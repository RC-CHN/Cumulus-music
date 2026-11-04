import { computed, ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import type {
  SystemMasterVolume,
  SystemMasterVolumeClient,
} from '../services/systemMasterVolume.ts';

export function useSystemMasterVolume(core: SystemMasterVolumeClient = useCore()) {
  const view = shallowRef<SystemMasterVolume>();
  const loading = ref(false);
  const visible = computed(() => !!view.value && view.value.kind !== 'unsupported');
  let disposed = false;
  async function read() {
    if (disposed || loading.value) return;
    loading.value = true;
    try {
      const result = await core.systemMasterVolume();
      if (!disposed) view.value = result;
    } catch {
      if (!disposed && visible.value) view.value = { kind: 'unavailable' };
    } finally {
      if (!disposed) loading.value = false;
    }
  }
  function dispose() {
    disposed = true;
  }
  return { view, visible, loading, read, dispose };
}
