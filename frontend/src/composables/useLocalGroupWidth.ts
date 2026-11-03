import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

const storageKey = 'cumulus.local.group-width.v1';
const minimum = 155;
const contentMinimum = 460;
const gap = 20;
const compact = 120;
const maximumPreference = 4096;

function readWidth() {
  try {
    const value = Number(localStorage.getItem(storageKey));
    if (Number.isFinite(value) && value >= minimum && value <= maximumPreference) return value;
  } catch {
    // The control still works if WebView preference storage is unavailable.
  }
  return minimum;
}

export function useLocalGroupWidth(
  container: Ref<HTMLElement | undefined>,
  grouped: () => boolean,
) {
  const preferred = ref(readWidth());
  const available = ref(0);
  const draft = ref<number>();
  const maximum = computed(() =>
    Math.min(maximumPreference, Math.floor(available.value) - contentMinimum - gap),
  );
  const enabled = computed(() => grouped() && maximum.value > minimum);
  const clamp = (value: number) => Math.round(Math.min(maximum.value, Math.max(minimum, value)));
  const width = computed(() =>
    maximum.value >= minimum ? clamp(draft.value ?? preferred.value) : compact,
  );
  let cancel: (() => void) | undefined;

  function commit(value: number) {
    preferred.value = clamp(value);
    try {
      localStorage.setItem(storageKey, String(preferred.value));
    } catch {
      // Keep the in-memory preference for this page.
    }
  }

  function key(event: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key) || !enabled.value) return;
    event.preventDefault();
    event.stopPropagation();
    cancel?.();
    const step = event.shiftKey ? 32 : 8;
    commit(
      event.key === 'Home'
        ? minimum
        : event.key === 'End'
          ? maximum.value
          : width.value + (event.key === 'ArrowLeft' ? -step : step),
    );
  }

  function start(event: PointerEvent) {
    if (!enabled.value || event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    cancel?.();
    const handle = event.currentTarget as HTMLElement;
    handle.focus({ preventScroll: true });
    const pointer = event.pointerId;
    const startWidth = width.value;
    const startX = event.clientX;
    let frame = 0;
    let latest = startX;
    draft.value = startWidth;
    const move = (e: PointerEvent) => {
      if (e.pointerId !== pointer) return;
      latest = e.clientX;
      if (!frame)
        frame = requestAnimationFrame(() => {
          frame = 0;
          draft.value = clamp(startWidth + latest - startX);
        });
    };
    const stop = () => {
      if (frame) cancelAnimationFrame(frame);
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', finish);
      document.removeEventListener('pointercancel', interrupted);
      document.removeEventListener('keydown', escape, true);
      handle.removeEventListener('lostpointercapture', interrupted);
      window.removeEventListener('blur', stop);
      cancel = undefined;
      draft.value = undefined;
      if (handle.hasPointerCapture(pointer)) handle.releasePointerCapture(pointer);
    };
    const finish = (e: PointerEvent) => {
      if (e.pointerId !== pointer) return;
      if (enabled.value && e.clientX !== startX) commit(startWidth + e.clientX - startX);
      stop();
    };
    const interrupted = (e: PointerEvent) => {
      if (e.pointerId === pointer) stop();
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopImmediatePropagation();
      stop();
    };
    cancel = stop;
    document.addEventListener('pointermove', move);
    document.addEventListener('pointerup', finish);
    document.addEventListener('pointercancel', interrupted);
    document.addEventListener('keydown', escape, true);
    handle.addEventListener('lostpointercapture', interrupted);
    window.addEventListener('blur', stop);
    try {
      handle.setPointerCapture(pointer);
    } catch {
      // A synthetic pointer still exercises the bounded document listeners.
    }
  }

  watch(
    container,
    (element, _, cleanup) => {
      cancel?.();
      if (!element) return;
      const measure = () => {
        const next = element.clientWidth;
        if (available.value !== next) cancel?.();
        available.value = next;
      };
      measure();
      const observer = new ResizeObserver(measure);
      observer.observe(element);
      cleanup(() => observer.disconnect());
    },
    { flush: 'post' },
  );
  watch(grouped, () => cancel?.());
  onBeforeUnmount(() => cancel?.());
  return { width, minimum, maximum, enabled, key, start };
}
