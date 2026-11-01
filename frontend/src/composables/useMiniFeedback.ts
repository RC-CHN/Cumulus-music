import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import { failure, type CoreFailure } from '../services/contracts.ts';
import type { FmReasons } from '../services/fm.ts';
import type { MiniClient, MiniFmFeedbackView, MiniView } from '../services/mini.ts';

interface Target {
  epoch: string;
  entryId: string;
  title: string;
  ticket: string;
}
export function useMiniFeedback(
  view: Ref<MiniView | null>,
  client: MiniClient,
  refresh: () => Promise<MiniView | undefined>,
  otherBusy = () => false,
) {
  const open = ref(false),
    writing = ref(false),
    reading = ref(false),
    loadingReasons = ref(false);
  const target = shallowRef<Target>(),
    state = shallowRef<MiniFmFeedbackView>(),
    reasons = shallowRef<FmReasons>();
  const issue = shallowRef<CoreFailure>(),
    readIssue = shallowRef<CoreFailure>(),
    reasonIssue = shallowRef<CoreFailure>();
  const observe = ref(false);
  let alive = true,
    serial = 0,
    reasonSerial = 0;
  const sameEntry = computed(
    () =>
      !!target.value &&
      target.value.epoch === view.value?.media.epoch &&
      target.value.entryId === view.value?.media.selection.audioEntryId &&
      view.value?.media.modeFamily === 'fm',
  );
  const changed = computed(() => !sameEntry.value || target.value?.ticket !== view.value?.ticket);
  const needsRead = computed(
    () => observe.value || changed.value || !!readIssue.value || !state.value,
  );
  const canSubmit = computed(
    () =>
      open.value &&
      !!state.value &&
      !state.value.pending &&
      !needsRead.value &&
      !writing.value &&
      !reading.value &&
      !otherBusy(),
  );
  const canOpen = computed(
    () =>
      !open.value &&
      !otherBusy() &&
      !!view.value?.media.selection.audioEntryId &&
      view.value?.media.modeFamily === 'fm',
  );
  function reset() {
    serial++;
    reasonSerial++;
    open.value = false;
    writing.value = false;
    reading.value = false;
    loadingReasons.value = false;
    target.value = undefined;
    state.value = undefined;
    reasons.value = undefined;
    issue.value = readIssue.value = reasonIssue.value = undefined;
    observe.value = false;
  }
  watch(() => view.value?.media.epoch, reset);
  onScopeDispose(() => {
    alive = false;
    reset();
  });
  const active = (request: number) =>
    alive && open.value && request === serial && target.value?.epoch === view.value?.media.epoch;
  async function loadReasons() {
    const displayed = target.value;
    if (!displayed || !canSubmit.value || loadingReasons.value) return;
    const request = ++reasonSerial;
    loadingReasons.value = true;
    reasonIssue.value = undefined;
    try {
      const value = await client.feedbackReasons(displayed.ticket);
      if (
        alive &&
        open.value &&
        request === reasonSerial &&
        sameEntry.value &&
        target.value?.ticket === displayed.ticket &&
        !changed.value &&
        value.entryId === displayed.entryId
      )
        reasons.value = value;
    } catch (error) {
      if (alive && open.value && request === reasonSerial && sameEntry.value)
        reasonIssue.value = failure(error);
    } finally {
      if (alive && request === reasonSerial) loadingReasons.value = false;
    }
  }
  async function readState(remote = true) {
    if (
      !target.value ||
      !sameEntry.value ||
      writing.value ||
      reading.value ||
      otherBusy() ||
      !view.value
    )
      return false;
    const displayed = target.value,
      ticket = remote ? view.value.ticket : displayed.ticket,
      request = ++serial;
    reading.value = true;
    readIssue.value = undefined;
    try {
      const value = await client.feedbackView(ticket, remote);
      if (!active(request)) return false;
      if (
        value.epoch !== displayed.epoch ||
        value.entryId !== displayed.entryId ||
        view.value?.ticket !== ticket
      )
        throw { code: 'stale', message: '播放状态已更新，请重新读取', retryable: true };
      target.value = { ...displayed, ticket };
      state.value = value;
      observe.value = false;
      return true;
    } catch (error) {
      if (active(request)) {
        readIssue.value = failure(error);
        observe.value = true;
      }
    } finally {
      if (active(request)) reading.value = false;
    }
    return false;
  }
  async function show() {
    const current = view.value;
    if (!current || !canOpen.value) return;
    target.value = {
      epoch: current.media.epoch,
      entryId: current.media.selection.audioEntryId!,
      title: current.media.item?.title ?? '',
      ticket: current.ticket,
    };
    open.value = true;
    if (await readState(false)) void loadReasons();
  }
  function close() {
    if (!writing.value) reset();
  }
  async function submit(key?: string) {
    const displayed = target.value;
    if (
      !displayed ||
      !canSubmit.value ||
      (key &&
        (reasons.value?.entryId !== displayed.entryId ||
          !reasons.value.options.some((r) => r.key === key)))
    )
      return false;
    const request = ++serial;
    writing.value = true;
    issue.value = undefined;
    reasonSerial++;
    loadingReasons.value = false;
    try {
      await client.feedbackSubmit(displayed.ticket, key);
      if (!active(request)) return false;
      reset();
      return true;
    } catch (error) {
      if (active(request)) {
        issue.value = failure(error);
        observe.value = true;
        // Observing the new native ticket never authorizes a repeated write.
        await refresh().catch(() => {});
      }
    } finally {
      if (active(request)) writing.value = false;
    }
    return false;
  }
  return {
    open,
    target,
    state,
    reasons,
    issue,
    readIssue,
    reasonIssue,
    writing,
    reading,
    loadingReasons,
    sameEntry,
    changed,
    needsRead,
    canSubmit,
    canOpen,
    show,
    close,
    submit,
    readState,
    loadReasons,
  };
}
