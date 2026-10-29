import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { ShortcutStatus, ShortcutStatusClient } from '../services/shortcuts.ts';
import { useCore } from '../services/core.ts';
export function useShortcutStatus(core: ShortcutStatusClient = useCore()) {
  const view = shallowRef<ShortcutStatus>(),
    error = ref('');
  let disposed = false,
    unlisten: (() => void) | undefined;
  function accept(value: ShortcutStatus) {
    if (
      disposed ||
      !/^\d+$/.test(value.revision) ||
      (view.value && BigInt(value.revision) < BigInt(view.value.revision))
    )
      return;
    view.value = value;
    error.value = '';
  }
  async function read() {
    try {
      accept(await core.shortcutStatus());
    } catch (e) {
      if (!disposed) error.value = failure(e).message;
    }
  }
  async function connect() {
    try {
      const stop = await core.onShortcuts(accept);
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
      await read();
    } catch (e) {
      if (!disposed) error.value = failure(e).message;
    }
  }
  function dispose() {
    disposed = true;
    unlisten?.();
  }
  return { view, error, read, connect, dispose };
}
