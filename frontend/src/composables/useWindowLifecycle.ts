import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  WindowLifecycleClient,
  WindowLifecycleView,
  WindowCloseCommand,
} from '../services/window.ts';
import { useCore } from '../services/core.ts';
export function useWindowLifecycle(core: WindowLifecycleClient = useCore()) {
  const view = shallowRef<WindowLifecycleView>();
  const error = ref(''),
    pending = ref(false);
  let disposed = false,
    unlisten: (() => void) | undefined;
  function accept(value: WindowLifecycleView) {
    if (disposed) return;
    // Events may overtake command/read replies; keep the native order, using
    // BigInt so long-running process counters never round through JS Number.
    if (!/^\d+$/.test(value.revision)) return;
    if (view.value && BigInt(value.revision) < BigInt(view.value.revision)) return;
    view.value = value;
  }
  async function read() {
    try {
      accept(await core.windowLifecycle());
    } catch (e) {
      if (!disposed) error.value = failure(e).message;
    }
  }
  async function connect() {
    try {
      const stop = await core.onWindowLifecycle(accept);
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
  async function command(value: WindowCloseCommand) {
    if (disposed || pending.value || view.value?.busy) return;
    pending.value = true;
    error.value = '';
    try {
      accept(await core.windowCloseCommand(value));
    } catch (e) {
      if (!disposed) {
        error.value = failure(e).message;
        await read();
      }
    } finally {
      if (!disposed) pending.value = false;
    }
  }
  function dispose() {
    disposed = true;
    unlisten?.();
  }
  return { view, error, pending, connect, read, command, dispose };
}
