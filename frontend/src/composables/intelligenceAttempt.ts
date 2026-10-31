import { watch, type Ref } from 'vue';
import type { PlayerView } from '../services/player.ts';
import type { PlayerModel } from './usePlayer.ts';
import { failure } from '../services/contracts.ts';

export interface IntelligenceAttempt {
  id: string;
  epoch: string;
  promise: Promise<PlayerView>;
  cancel(): Promise<void>;
  cancelRequested?(): boolean;
}
export interface IntelligenceCycle {
  eligible(): boolean;
  pending(): boolean;
  enter(revision: string): IntelligenceAttempt;
  idle(epoch: string, signal: AbortSignal): Promise<void>;
}
export const staleMode = () => ({
  code: 'stale',
  message: '播放来源或账号已变化',
  retryable: false,
});

export function intelligenceIdle(
  busy: Ref<boolean>,
  player: PlayerModel,
  epoch: string,
  signal: AbortSignal,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let done = false;
    const stop = watch([busy, player.ready, () => player.view.value?.epoch], check, {
      flush: 'sync',
    });
    function finish(error?: unknown) {
      if (done) return;
      done = true;
      stop();
      signal.removeEventListener('abort', abort);
      if (error) reject(error);
      else resolve();
    }
    function abort() {
      finish(staleMode());
    }
    function check() {
      if (signal.aborted || !player.ready.value || player.view.value?.epoch !== epoch) abort();
      else if (!busy.value) finish();
    }
    signal.addEventListener('abort', abort, { once: true });
    check();
  });
}

/** A matching host-supplied UUID proves this request actually committed its
 * seed. A phase change alone could belong to another window or stale request.
 * Only a cancelled, uncommitted attempt may continue on its original queue.
 */
export function intelligenceBoundary(
  player: PlayerModel,
  attempt: IntelligenceAttempt,
  revision: string,
  signal: AbortSignal,
  cancelled: () => boolean,
): Promise<PlayerView> {
  return new Promise((resolve, reject) => {
    let done = false;
    const stop = watch([player.view, player.ready], observe, { flush: 'sync' });
    function finish(view?: PlayerView, error?: unknown) {
      if (done) return;
      done = true;
      stop();
      signal.removeEventListener('abort', abort);
      if (error) reject(error);
      else resolve(view!);
    }
    function abort() {
      finish(undefined, staleMode());
    }
    function current() {
      const view = player.view.value;
      if (signal.aborted || !player.ready.value || view?.epoch !== attempt.epoch) {
        abort();
        return;
      }
      return view;
    }
    function owned(view: PlayerView) {
      return view.intelligenceOperation === attempt.id && !!view.intelligence;
    }
    function observe() {
      const view = current();
      if (view && owned(view)) finish(view);
    }
    signal.addEventListener('abort', abort, { once: true });
    // Always attach both handlers, even if the watch has already acknowledged
    // the seed. Later HTTP failures belong to the shared intelligence model.
    void attempt.promise.then(
      (result) => {
        if (done) return;
        const view = current();
        if (!view) return;
        if (owned(view)) finish(view);
        else if (view.queueRevision === result.queueRevision && owned(result)) finish(result);
        else finish(undefined, staleMode());
      },
      async (error) => {
        if (done) return;
        if (cancelled() && failure(error).code === 'stale') {
          try {
            const view = await player.syncPlayback();
            if (done) return;
            if (view.epoch === attempt.epoch && (owned(view) || view.queueRevision === revision))
              finish(view);
            else abort();
          } catch (problem) {
            finish(undefined, problem);
          }
        } else finish(undefined, error);
      },
    );
    observe();
  });
}
