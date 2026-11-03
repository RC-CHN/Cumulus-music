import { computed, nextTick, onBeforeUnmount, shallowRef, watch, type Ref } from 'vue';
import { lyricAnchor, lyricOffsets, lyricRange, type LyricAnchor } from '../services/lyricWindow';

export function useLyricWindow(
  panel: Ref<HTMLElement | undefined>,
  body: Ref<HTMLElement | undefined>,
  estimates: () => readonly number[],
) {
  const offsets = shallowRef<number[]>([0]);
  const range = shallowRef({ start: 0, end: 0 });
  const pinned = shallowRef<number>();
  const measured = new Map<number, number>();
  let width = 0;
  let settledAnchor: LyricAnchor | undefined;
  let resize: ResizeObserver | undefined;
  let observed = new Set<Element>();
  let frame: number | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const height = computed(() => offsets.value.at(-1) ?? 0);
  const rendered = computed(() => {
    const { start, end } = range.value;
    const rows = Array.from({ length: end - start }, (_, i) => start + i);
    const focus = pinned.value;
    if (focus !== undefined && focus < offsets.value.length - 1 && (focus < start || focus >= end))
      rows.splice(focus < start ? 0 : rows.length, 0, focus);
    return rows;
  });

  function localTop() {
    if (!panel.value || !body.value) return 0;
    return panel.value.getBoundingClientRect().top - body.value.getBoundingClientRect().top;
  }
  function anchor() {
    return body.value ? lyricAnchor(offsets.value, localTop()) : undefined;
  }
  function cancel() {
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (deadline !== undefined) clearTimeout(deadline);
    frame = deadline = undefined;
  }
  function refresh() {
    cancel();
    const outer = panel.value;
    const list = body.value;
    if (!outer || !list) return;
    const top = localTop();
    const measuredWidth = list.clientWidth;
    const widthChanged = width > 0 && width !== measuredWidth;
    // WebKit can scroll its focused element during a resize, before the
    // ResizeObserver callback. Keep the last anchor from the previous width.
    const saved = (widthChanged ? settledAnchor : undefined) ?? lyricAnchor(offsets.value, top);
    if (width !== measuredWidth) {
      width = measuredWidth;
      measured.clear();
    }
    for (const row of observed) {
      const node = row as HTMLElement;
      const index = Number(node.dataset.lyricIndex);
      const size =
        node.getBoundingClientRect().height +
        (parseFloat(getComputedStyle(node).marginBottom) || 0);
      if (node.parentElement === list && Number.isInteger(index) && size > 0)
        measured.set(index, size);
    }
    const next = lyricOffsets(estimates().map((estimate, i) => measured.get(i) ?? estimate));
    let adjusted = top;
    if (saved && saved.offset >= 0 && saved.index + 1 < next.length) {
      const size = next[saved.index + 1]! - next[saved.index]!;
      adjusted = next[saved.index]! + Math.min(saved.offset, Math.max(0, size - 1));
      if (Math.abs(adjusted - top) > 0.5) {
        const desired = outer.scrollTop + adjusted - top;
        outer.scrollTop = desired;
        const applied = outer.scrollTop;
        if (Math.abs(applied - desired) > 0.5)
          void nextTick(() => {
            if (outer === panel.value && list === body.value && outer.scrollTop === applied)
              outer.scrollTop = desired;
          });
      }
    }
    if (next.length !== offsets.value.length || next.some((v, i) => v !== offsets.value[i]))
      offsets.value = next;
    settledAnchor = lyricAnchor(next, adjusted);
    const result = lyricRange(next, adjusted, outer.clientHeight);
    if (result.start !== range.value.start || result.end !== range.value.end) range.value = result;
  }
  function schedule() {
    if (frame !== undefined) return;
    frame = requestAnimationFrame(refresh);
    deadline = setTimeout(refresh, 64);
  }
  function restore(saved: LyricAnchor) {
    const outer = panel.value;
    if (!outer || !body.value) return;
    refresh();
    const start = offsets.value[saved.index];
    const end = offsets.value[saved.index + 1];
    if (start === undefined || end === undefined) return;
    const within = saved.offset < 0 ? saved.offset : Math.min(saved.offset, end - start - 1);
    outer.scrollTop += start + within - localTop();
    refresh();
  }
  function focus(index?: number) {
    pinned.value = index;
  }
  watch(
    [panel, body],
    ([outer, list]) => {
      resize?.disconnect();
      observed.clear();
      measured.clear();
      width = 0;
      settledAnchor = undefined;
      offsets.value = [0];
      range.value = { start: 0, end: 0 };
      pinned.value = undefined;
      cancel();
      if (!outer || !list) return;
      resize = new ResizeObserver(schedule);
      resize.observe(outer);
      refresh();
    },
    { flush: 'post' },
  );
  watch(
    [rendered, body],
    () => {
      const next = new Set<Element>(body.value?.children ?? []);
      for (const node of observed) if (!next.has(node)) resize?.unobserve(node);
      for (const node of next) if (!observed.has(node)) resize?.observe(node);
      observed = next;
      schedule();
    },
    { flush: 'post' },
  );
  watch(estimates, schedule, { flush: 'post' });
  onBeforeUnmount(() => {
    resize?.disconnect();
    cancel();
  });
  return { offsets, height, rendered, anchor, restore, focus, schedule };
}
