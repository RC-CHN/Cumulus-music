import { computed, ref, watch, type Ref } from 'vue';
import type { MiniClient, MiniPlaybackReceipt, MiniView } from '../services/mini.ts';
import { failure, type CoreFailure } from '../services/contracts.ts';
import { useIntelligence, type IntelligenceControl } from './useIntelligence.ts';
import { usePlaybackMode, type ModePlaybackControl } from './usePlaybackMode.ts';
import { intelligenceIdle, staleMode } from './intelligenceAttempt.ts';

/** A narrow adapter over the second window's own snapshots and exact native
 * acknowledgements. It never invokes main-window player/account commands. */
export function useMiniModes(
  view: Ref<MiniView | null>,
  client: MiniClient,
  refresh: () => Promise<MiniView | undefined>,
) {
  const projection = computed<MiniPlaybackReceipt | undefined>(() => {
    const media = view.value?.media;
    if (!media?.mode || !media.queueRevision || !media.modeFamily) return;
    return {
      epoch: media.epoch,
      queueRevision: media.queueRevision,
      playId: media.selection.audioPlayId,
      mode: media.mode,
      fm: false,
      intelligence: media.intelligence,
      intelligenceOperation: media.intelligenceOperation ?? null,
    };
  });
  const ready = computed(() => !!projection.value);
  const together = () => view.value?.media.modeFamily === 'together';
  function captured(revision: string) {
    const value = view.value;
    if (!value || value.media.queueRevision !== revision || !ready.value) throw staleMode();
    return value;
  }
  async function synchronize() {
    await refresh();
    const value = projection.value;
    if (!value) throw staleMode();
    return value;
  }
  async function acknowledge(work: () => Promise<MiniPlaybackReceipt>, cancellable = false) {
    let receipt: MiniPlaybackReceipt;
    try {
      receipt = await work();
    } catch (error) {
      // Refresh the safe projection before classifying a stale write or
      // offering a retry. Explicit cancellation already has its own readback.
      if (!cancellable || !local.cancelling.value) await refresh().catch(() => {});
      throw error;
    }
    await refresh();
    return receipt;
  }
  const port: ModePlaybackControl<MiniPlaybackReceipt> & IntelligenceControl<MiniPlaybackReceipt> =
    {
      view: projection,
      ready,
      syncPlayback: synchronize,
      configureMode: async (mode, revision) => {
        const displayed = captured(revision);
        return acknowledge(() => client.mode(displayed.ticket, mode));
      },
      intelligence: async (command) => {
        const displayed = captured(command.expected_queue_revision);
        if (command.kind === 'start' && command.origin.kind !== 'current') throw staleMode();
        return acknowledge(
          () =>
            client.intelligence(displayed.ticket, {
              kind: command.kind,
              operation_id: command.operation_id,
            }),
          true,
        );
      },
    };
  const local = useIntelligence(port, together, {
    playerIntelligenceCancel: (epoch, id) => client.cancelIntelligence(epoch, id),
  });
  const remoteCancelling = ref(false),
    remoteIssue = ref<CoreFailure>();
  const busy = computed(() => local.busy.value || !!view.value?.media.intelligenceRead);
  const cancelling = computed(() => local.cancelling.value || remoteCancelling.value);
  const preparing = computed(() =>
    local.busy.value
      ? local.progress.value === 'preparing'
      : !!view.value?.media.intelligenceRead?.preparing,
  );
  const resumable = computed(() => local.resumable.value && !busy.value);
  const dismissedFallback = ref<string>();
  const fallbackKey = computed(() => {
    const media = view.value?.media;
    return media?.intelligence === 'fallback'
      ? `${media.epoch}:${media.intelligenceOperation ?? media.selection.audioPlayId}`
      : undefined;
  });
  const canStart = () => local.canStart.value && !busy.value && !!view.value?.media.canIntelligence;
  const mode = usePlaybackMode(port, together, {
    eligible: () => !!view.value?.media.canIntelligence && !together(),
    pending: () => busy.value,
    enter: (revision) => local.enterCurrent(revision),
    idle: (epoch, signal) => intelligenceIdle(busy, port, epoch, signal),
  });
  watch(
    () => view.value?.media.epoch,
    () => {
      remoteCancelling.value = false;
      remoteIssue.value = undefined;
    },
  );
  watch(
    () => view.value?.media.intelligenceRead?.operationId,
    () => {
      remoteCancelling.value = false;
    },
  );
  async function cancel() {
    if (local.busy.value) return local.cancel();
    const displayed = view.value,
      activity = displayed?.media.intelligenceRead;
    if (!displayed || !activity || remoteCancelling.value) return;
    remoteCancelling.value = true;
    remoteIssue.value = undefined;
    try {
      await client.cancelIntelligence(displayed.media.epoch, activity.operationId);
      await refresh();
    } catch (error) {
      if (view.value?.media.epoch === displayed.media.epoch) remoteIssue.value = failure(error);
      remoteCancelling.value = false;
    }
  }
  const issue = computed<CoreFailure | undefined>(
    () =>
      mode.issue.value ??
      local.issue.value ??
      remoteIssue.value ??
      (fallbackKey.value && fallbackKey.value !== dismissedFallback.value
        ? {
            code: 'intelligence_failed',
            message: '心动模式开启失败，已返回列表循环',
            retryable: true,
          }
        : undefined),
  );
  function dismiss() {
    mode.issue.value = undefined;
    local.dismiss();
    remoteIssue.value = undefined;
    dismissedFallback.value = fallbackKey.value;
  }
  function retry() {
    if (!issue.value) return;
    if (mode.issue.value) mode.retry();
    else if (resumable.value) void local.resume();
    else if (canStart()) void local.start({ kind: 'current' });
  }
  return {
    mode,
    busy,
    cancelling,
    preparing,
    resumable,
    issue,
    cancel,
    dismiss,
    retry,
    resume: () => local.resume(),
    canRetry: computed(() =>
      mode.issue.value
        ? mode.canRetry.value
        : issue.value?.retryable !== false && (resumable.value || canStart()),
    ),
  };
}
