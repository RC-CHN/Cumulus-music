import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import type { CoreFailure } from '../services/contracts.ts';
import type { FmModel } from './useFm.ts';

interface Playback {
  epoch: string;
  entryId: string | null | undefined;
  fm: boolean;
}
interface Target {
  id: number;
  epoch: string;
  source: string;
  entryId: string;
  name: string;
}

/** A dialog owns the displayed entry, never whichever song finishes loading later. */
export function useFmFeedback(model: FmModel, playback: () => Playback | undefined) {
  const open = ref(false),
    pending = ref(false),
    target = shallowRef<Target>(),
    issue = shallowRef<CoreFailure>();
  let serial = 0,
    nextId = 0,
    alive = true;
  const source = () => JSON.stringify(model.source.value);
  const accountCurrent = computed(() => target.value?.epoch === playback()?.epoch);
  const changed = computed(() => {
    const value = target.value,
      actual = playback();
    return (
      !value ||
      !accountCurrent.value ||
      value.source !== source() ||
      !actual?.fm ||
      actual.entryId !== value.entryId ||
      !model.view.value?.active ||
      model.view.value.current !== value.entryId
    );
  });
  const busy = computed(() => pending.value || model.busy.value);
  const review = computed(() => !!model.view.value?.operations.some((op) => op.kind !== 'skip'));
  const canSubmit = computed(
    () => open.value && !busy.value && !changed.value && !review.value && !model.stateIssue.value,
  );
  function reset() {
    serial++;
    open.value = false;
    pending.value = false;
    issue.value = undefined;
    model.clearReasons();
  }
  watch([() => playback()?.epoch, source], reset);
  onScopeDispose(() => {
    alive = false;
    reset();
  });
  function show() {
    const actual = playback(),
      current = model.view.value?.items.find((item) => item.entryId === model.view.value?.current);
    if (
      open.value ||
      busy.value ||
      model.stateIssue.value ||
      !actual?.fm ||
      !current ||
      actual.entryId !== current.entryId
    )
      return;
    serial++;
    target.value = {
      id: ++nextId,
      epoch: actual.epoch,
      source: source(),
      entryId: current.entryId,
      name: current.track.name,
    };
    issue.value = undefined;
    open.value = true;
    void model.loadReasons(current.entryId);
  }
  function close() {
    if (!busy.value) reset();
  }
  function retryReasons() {
    if (
      !open.value ||
      changed.value ||
      busy.value ||
      model.stateIssue.value ||
      model.reasonsLoading.value ||
      !target.value
    )
      return;
    void model.loadReasons(target.value.entryId);
  }
  async function retryState() {
    if (!open.value || busy.value || model.loading.value || !model.stateIssue.value) return false;
    return model.load(true);
  }
  async function submit(reasonKey?: string) {
    const displayed = target.value;
    if (!displayed || !canSubmit.value) return false;
    if (
      reasonKey &&
      (model.reasons.value?.entryId !== displayed.entryId ||
        !model.reasons.value.options.some((reason) => reason.key === reasonKey))
    )
      return false;
    const attempt = ++serial;
    pending.value = true;
    issue.value = undefined;
    try {
      const result = await model.command(
        reasonKey
          ? { kind: 'feedback', entry_id: displayed.entryId, reason_key: reasonKey }
          : { kind: 'trash', entry_id: displayed.entryId },
      );
      if (!alive || attempt !== serial || displayed !== target.value || !accountCurrent.value)
        return false;
      if (result?.ok) {
        open.value = false;
        model.clearReasons();
        return true;
      }
      issue.value = result?.issue;
      return false;
    } finally {
      if (attempt === serial) pending.value = false;
    }
  }
  return {
    open,
    target,
    pending,
    issue,
    accountCurrent,
    changed,
    busy,
    review,
    canSubmit,
    show,
    close,
    retryReasons,
    retryState,
    submit,
  };
}
