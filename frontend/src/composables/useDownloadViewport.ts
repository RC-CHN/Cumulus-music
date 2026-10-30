import { nextTick, onBeforeUnmount, watch, type Ref } from 'vue';
import type { DownloadModel } from './useDownloads';
import { downloadViewportKey, type DownloadViewport } from '../services/downloadViewport';

const rows = '[data-download-task],[data-download-mv]';
const taskId = (row: HTMLElement) => row.dataset.downloadTask ?? row.dataset.downloadMv;
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

export function useDownloadViewport(
  root: Ref<HTMLElement | undefined>,
  model: DownloadModel,
  growth: Ref<number[]>,
) {
  let pending = model.viewport.value;
  let ownerEpoch = model.view.value?.epoch;
  let lastFocus: DownloadViewport['focus'];
  let mounted = true,
    restoring = false,
    cancelled = false;
  const key = () => downloadViewportKey(model.query.value);
  function focus(event: FocusEvent) {
    const owner = event.target as HTMLElement;
    const row = owner.closest<HTMLElement>(rows);
    lastFocus =
      row && taskId(row) ? { task: taskId(row)!, action: owner.dataset.downloadAction } : undefined;
  }
  function cancel() {
    cancelled = true;
    pending = undefined;
  }
  watch(key, () => {
    cancel();
    lastFocus = undefined;
  });
  watch(
    () => model.view.value?.epoch,
    (epoch) => {
      if (ownerEpoch && epoch !== ownerEpoch) cancel();
      else if (epoch) ownerEpoch = epoch;
    },
  );
  const scroller = () =>
    root.value?.querySelector<HTMLElement>('.download-scroll,.download-mv-scroll');
  async function restore() {
    const saved = pending,
      view = model.view.value;
    if (cancelled || restoring || !saved || !view || model.loading.value || !scroller()) return;
    if (saved.epoch !== view.epoch || saved.query !== key()) {
      cancel();
      return;
    }
    restoring = true;
    try {
      const owner = document.activeElement;
      const valid = () =>
        mounted &&
        !cancelled &&
        pending === saved &&
        saved.epoch === model.view.value?.epoch &&
        saved.query === key();
      // ResizeObserver establishes the actual column count before applying an anchor.
      await frame();
      await frame();
      await nextTick();
      if (!valid()) return;
      const el = scroller()!;
      const items = model.view.value!.tasks;
      let index = items.findIndex((task) => task.id === saved.anchor);
      if (index < 0) index = Math.min(saved.index, items.length - 1);
      const columns = Number(el.dataset.downloadColumns ?? 1);
      const pitch = Number(el.dataset.downloadPitch ?? 64);
      el.scrollTop = Math.max(
        0,
        Math.floor(index / columns) * pitch + Math.min(saved.offset, pitch - 1),
      );
      el.dispatchEvent(new Event('scroll'));
      if (saved.growth.length === growth.value.length) growth.value = [...saved.growth];
      await frame();
      await frame();
      await nextTick();
      if (!valid()) return;
      const candidates = [...el.querySelectorAll<HTMLElement>(rows)];
      const selected = candidates.find((row) => taskId(row) === saved.focus?.task);
      const controls = [
        ...(selected?.querySelectorAll<HTMLElement>('[data-download-action]') ?? []),
      ];
      const target =
        controls.find((control) => control.dataset.downloadAction === saved.focus?.action) ??
        selected;
      // A deliberate action during restoration owns focus; never steal it back.
      if (
        saved.focus &&
        (document.activeElement === owner || document.activeElement === document.body)
      ) {
        const r = target?.getBoundingClientRect(),
          bounds = el.getBoundingClientRect();
        if (
          r &&
          r.bottom > bounds.top &&
          r.top < bounds.bottom &&
          Math.min(r.bottom, bounds.bottom) - Math.max(r.top, bounds.top) < Math.min(32, r.height)
        ) {
          // A new error/notice can reduce the viewport. A one-pixel focus edge
          // is not an operable return target; reveal that control with minimal scroll.
          el.scrollTop += r.top < bounds.top ? r.top - bounds.top : r.bottom - bounds.bottom;
          el.dispatchEvent(new Event('scroll'));
        }
        (r && r.bottom > bounds.top && r.top < bounds.bottom && target?.isConnected
          ? target
          : el
        )?.focus({
          preventScroll: true,
        });
      }
      pending = undefined;
    } finally {
      restoring = false;
    }
  }
  watch([root, model.view, model.loading], () => void restore(), { flush: 'post' });
  onBeforeUnmount(() => {
    mounted = false;
    const el = scroller(),
      view = model.view.value;
    if (!el || !view || view.epoch !== ownerEpoch || restoring || !el.clientHeight) return;
    const bounds = el.getBoundingClientRect();
    const anchor = [...el.querySelectorAll<HTMLElement>(rows)].find(
      (row) => row.getBoundingClientRect().bottom > bounds.top,
    );
    if (!anchor || !taskId(anchor)) return;
    const pitch = Number(el.dataset.downloadPitch ?? 64);
    const columns = Number(el.dataset.downloadColumns ?? 1);
    const index = Number(anchor.dataset.downloadIndex ?? anchor.dataset.mvIndex);
    model.saveViewport({
      epoch: view.epoch,
      query: key(),
      anchor: taskId(anchor)!,
      index,
      offset: el.scrollTop - Math.floor(index / columns) * pitch,
      focus: lastFocus,
      growth: [...growth.value],
    });
  });
  return { focus, cancel };
}
