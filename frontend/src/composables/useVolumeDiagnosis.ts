import { onScopeDispose, shallowRef, watch } from 'vue';
import type {
  VolumeDiagnosisClient,
  VolumeDiagnosisDismissal,
  VolumeDiagnosisVisit,
} from '../services/systemMasterVolume.ts';

export function useVolumeDiagnosis(
  core: VolumeDiagnosisClient,
  owner: () => string | undefined,
  autoClose: () => void,
) {
  const prompt = shallowRef<{ epoch: string; receipt: string }>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: symbol | undefined;

  function close(action: VolumeDiagnosisDismissal) {
    const current = prompt.value;
    if (!current) return;
    clearTimeout(timer);
    prompt.value = undefined;
    void core.dismissVolumeDiagnosis(current.epoch, current.receipt, action).catch(() => {});
    return current.epoch === owner() ? current.epoch : undefined;
  }

  async function check(visit: VolumeDiagnosisVisit) {
    if (visit.epoch !== owner() || pending || prompt.value) return;
    const request = Symbol();
    pending = request;
    try {
      const receipt = await core.checkVolumeDiagnosis(visit);
      if (pending !== request || !receipt || owner() !== visit.epoch) return;
      prompt.value = { epoch: visit.epoch, receipt };
      timer = setTimeout(autoClose, 5_000);
    } catch {
      // An unavailable diagnosis should not affect playback or slider feedback.
    } finally {
      if (pending === request) pending = undefined;
    }
  }

  function reset() {
    clearTimeout(timer);
    prompt.value = pending = undefined;
  }
  watch(owner, reset, { flush: 'sync' });
  onScopeDispose(reset);
  return { prompt, check, close };
}
