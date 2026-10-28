import { nextTick, onUnmounted } from 'vue';

// Native disabled controls may lose focus while IPC is pending. Restore only
// focus owned by this action; another account, action, dialog or control wins.
export function useControlFocus(
  owner: () => unknown,
  fallback: () => HTMLElement | null | undefined,
) {
  let disposed = false,
    serial = 0;
  onUnmounted(() => {
    disposed = true;
    serial++;
  });
  function capture(trigger: EventTarget | null | undefined) {
    const focused = trigger instanceof HTMLElement && document.activeElement === trigger;
    if (!focused) return () => {};
    const captured = owner(),
      current = ++serial;
    return () => {
      if (
        !disposed &&
        current === serial &&
        captured === owner() &&
        (document.activeElement === document.body ||
          // WebKit can retain a disabled input as active until its next render.
          // Move focus now; waiting for its delayed blur strands it on body.
          (document.activeElement === trigger && trigger.matches(':disabled')))
      ) {
        const target = trigger.isConnected && !trigger.matches(':disabled') ? trigger : fallback();
        if (
          target?.isConnected &&
          !target.matches(':disabled') &&
          (!document.querySelector('dialog[open]') || target.closest('dialog[open]'))
        ) {
          target.focus({ preventScroll: true });
          if (target !== trigger) target.scrollIntoView({ block: 'nearest' });
        }
      }
    };
  }
  async function run<T>(event: Event, action: () => T | Promise<T>): Promise<T> {
    const restore = capture(event.currentTarget);
    try {
      return await action();
    } finally {
      await nextTick();
      restore();
    }
  }
  return { run, capture };
}
