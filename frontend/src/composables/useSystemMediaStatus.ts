import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { SystemMediaStatusClient, SystemMediaStatus } from '../services/systemMedia.ts';
import { useCore } from '../services/core.ts';
export function useSystemMediaStatus(core: SystemMediaStatusClient = useCore()) {
  const view = shallowRef<SystemMediaStatus>(),
    error = ref('');
  let disposed = false,
    unlisten: (() => void) | undefined;
  function accept(value: SystemMediaStatus) {
    if (disposed || !/^\d+$/.test(value.revision)) return;
    if (view.value && BigInt(value.revision) < BigInt(view.value.revision)) return;
    view.value = value;
    error.value = '';
  }
  async function read() {
    try {
      accept(await core.systemMediaStatus());
    } catch (e) {
      if (!disposed) error.value = failure(e).message;
    }
  }
  async function connect() {
    try {
      const stop = await core.onSystemMediaStatus(accept);
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
  return { view, error, connect, read, dispose };
}
