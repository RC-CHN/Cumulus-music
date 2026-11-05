import { onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import type { CommentItem, CommentOperation, VinylCommentLikeAction } from '../services/comments';
import { failure } from '../services/contracts.ts';
import type { PlayerClient } from '../services/player';

export type VinylCommentLikeClient = Pick<
  PlayerClient,
  'playerVinylCommentLike' | 'playerVinylCommentLikeOperations'
>;

export function useVinylCommentLikes(
  api: VinylCommentLikeClient,
  target: Readonly<Ref<string | undefined>>,
  visible: Readonly<Ref<boolean>>,
  loaded: Readonly<Ref<boolean>>,
  contains: (item: CommentItem) => boolean,
  apply: (item: CommentItem) => void,
) {
  const operations = shallowRef(new Map<string, CommentOperation>());
  const busy = ref<string>();
  const ready = ref(false);
  const reading = ref(false);
  const readFailed = ref(false);
  const issue = shallowRef<{ id: string; message: string }>();
  let generation = 0;
  let readingVersion = 0;

  function reset() {
    generation++;
    readingVersion++;
    operations.value = new Map();
    busy.value = undefined;
    ready.value = false;
    reading.value = false;
    readFailed.value = false;
    issue.value = undefined;
  }
  watch(target, reset, { flush: 'sync' });
  onScopeDispose(reset);

  async function refresh() {
    const key = target.value;
    if (!key || reading.value) return;
    const [epoch, playId] = JSON.parse(key) as [string, string];
    const owner = generation;
    const started = ++readingVersion;
    const current = () =>
      owner === generation && key === target.value && started === readingVersion;
    reading.value = true;
    readFailed.value = false;
    try {
      const result = await api.playerVinylCommentLikeOperations(epoch, playId);
      if (!current()) return;
      if (result.epoch !== epoch) throw new Error('stale vinyl comment operation list');
      operations.value = new Map(result.value.map((row) => [row.id, row]));
      ready.value = true;
    } catch {
      if (current()) {
        ready.value = false;
        readFailed.value = true;
      }
    } finally {
      if (current()) reading.value = false;
    }
  }
  watch(
    [target, visible, loaded],
    ([key, active, list]) => {
      if (key && active && list && !busy.value) void refresh();
    },
    { immediate: true },
  );

  async function mutate(item: CommentItem, action: VinylCommentLikeAction) {
    const key = target.value;
    if (!key || !visible.value || !contains(item) || busy.value || !ready.value || reading.value)
      return;
    const pending = operations.value.has(item.id);
    if (action.kind === 'set' ? pending || item.liked === null : !pending) return;
    const [epoch, playId] = JSON.parse(key) as [string, string];
    const owner = generation;
    const current = () => owner === generation && key === target.value;
    busy.value = item.id;
    issue.value = undefined;
    try {
      const result = await api.playerVinylCommentLike(epoch, playId, item.id, action);
      if (!current()) return;
      if (result.epoch !== epoch || result.value.id !== item.id)
        throw new Error('stale vinyl comment operation');
      const operation = result.value;
      if (operation.phase !== 'confirmed' && operation.phase !== 'unchanged')
        throw new Error('vinyl comment like is not confirmed');
      if (
        !operation.observed ||
        operation.observed.id !== item.id ||
        operation.observed.liked !== operation.desired ||
        (action.kind === 'set' && operation.desired !== action.desired)
      )
        throw new Error('vinyl comment like has no observation');
      const remaining = new Map(operations.value);
      remaining.delete(item.id);
      operations.value = remaining;
      // An explicit action may complete while lyrics are briefly visible;
      // preserve the result for reopening the same play, never another play.
      apply(operation.observed);
    } catch (error) {
      if (current()) {
        const problem = failure(error);
        ready.value = false;
        await refresh();
        if (current())
          issue.value = {
            id: item.id,
            message: operations.value.has(item.id) ? '点赞结果尚未确认，请核对状态。' : problem.message,
          };
      }
    } finally {
      if (current()) busy.value = undefined;
    }
  }
  return {
    operations,
    busy,
    ready,
    reading,
    readFailed,
    issue,
    refresh,
    toggle: (item: CommentItem) => mutate(item, { kind: 'set', desired: item.liked !== true }),
    reconcile: (item: CommentItem) => mutate(item, { kind: 'reconcile' }),
  };
}
