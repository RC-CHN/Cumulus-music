import { computed, ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import type {
  SystemMasterVolumeClient,
  SystemSpatialSoundClient,
} from '../services/systemMasterVolume.ts';

function useOutputObservation<View extends { kind: 'unsupported' | 'unavailable' | 'ready' }>(
  observe: () => Promise<View>,
) {
  const view = shallowRef<View | { kind: 'unavailable' }>();
  const loading = ref(false);
  const visible = computed(() => !!view.value && view.value.kind !== 'unsupported');
  let disposed = false;
  async function read() {
    if (disposed || loading.value) return;
    loading.value = true;
    try {
      const result = await observe();
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

export function useSystemMasterVolume(core: SystemMasterVolumeClient = useCore()) {
  return useOutputObservation(() => core.systemMasterVolume());
}

export function useSystemSpatialSound(core: SystemSpatialSoundClient = useCore()) {
  return useOutputObservation(() => core.systemSpatialSound());
}
