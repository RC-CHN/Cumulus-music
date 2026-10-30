import { nextTick, onMounted, onUnmounted, ref, type Ref } from 'vue';

/** Keep the catalogue anchored while reserving the titlebar and player controls. */
export function useTagPopover(
  panel: Ref<HTMLElement | undefined>,
  anchor: () => HTMLElement | undefined,
  dismiss: () => void,
) {
  const style = ref({
    left: '0px',
    top: '0px',
    maxHeight: '374px',
    visibility: 'hidden' as 'hidden' | 'visible',
  });
  let frame = 0,
    alive = true;
  let observer: ResizeObserver | undefined;
  function position() {
    frame = 0;
    const target = anchor(),
      popup = panel.value;
    if (!alive || !target?.isConnected || !popup) return;
    const bounds = target.getBoundingClientRect();
    const top = Math.max(
      8,
      (document.querySelector('.topbar')?.getBoundingClientRect().bottom ?? 72) + 8,
    );
    const bottom = Math.min(
      innerHeight - 8,
      (document.querySelector('.playerbar')?.getBoundingClientRect().top ?? innerHeight) - 8,
    );
    const maxHeight = Math.max(120, bottom - top);
    const height = Math.min(374, maxHeight),
      width = Math.min(574, innerWidth - 16);
    const below = bounds.bottom + 6;
    const rightAligned = bounds.right - width;
    const preferredLeft = rightAligned < 8 ? bounds.left : rightAligned;
    const y =
      below + height <= bottom
        ? below
        : Math.max(top, Math.min(bounds.top - height - 6, bottom - height));
    style.value = {
      left: `${Math.max(8, Math.min(preferredLeft, innerWidth - width - 8))}px`,
      top: `${y}px`,
      maxHeight: `${maxHeight}px`,
      visibility: 'visible',
    };
  }
  function schedule(event?: Event) {
    if (event?.target instanceof Node && panel.value?.contains(event.target)) return;
    if (!frame) frame = requestAnimationFrame(position);
  }
  function outside(event: Event) {
    const target = event.target;
    if (target instanceof Node && !panel.value?.contains(target) && !anchor()?.contains(target))
      dismiss();
  }
  onMounted(async () => {
    await nextTick();
    if (!alive) return;
    position();
    await nextTick();
    if (!alive) return;
    panel.value
      ?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
      ?.focus({ preventScroll: true });
    if (!panel.value?.contains(document.activeElement)) panel.value?.focus({ preventScroll: true });
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('focusin', outside, true);
    document.addEventListener('scroll', schedule, true);
    window.addEventListener('resize', schedule);
    observer = new ResizeObserver(() => schedule());
    if (anchor()) observer.observe(anchor()!);
  });
  onUnmounted(() => {
    alive = false;
    cancelAnimationFrame(frame);
    observer?.disconnect();
    document.removeEventListener('pointerdown', outside, true);
    document.removeEventListener('focusin', outside, true);
    document.removeEventListener('scroll', schedule, true);
    window.removeEventListener('resize', schedule);
  });
  return style;
}
