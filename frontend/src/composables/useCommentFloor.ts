import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { resourceKey, type CommentResource, type CommentItem } from '../services/comments.ts';
import type {
  CommentFloorClient,
  CommentFloorPage,
  CommentFloorQuery,
} from '../services/commentFloor.ts';
export function useCommentFloor(core: CommentFloorClient = useCore(), expired = () => {}) {
  const page = shallowRef<CommentFloorPage>(),
    query = shallowRef<CommentFloorQuery>(),
    history = shallowRef<CommentFloorQuery[]>([]),
    loading = ref(false),
    error = ref('');
  let epoch = '',
    resource: CommentResource | undefined,
    generation = 0,
    reading = 0;
  const live = (e: string, g: number, n: number) =>
    !!epoch && epoch === e && generation === g && reading === n;
  function close() {
    generation++;
    reading++;
    page.value = undefined;
    query.value = undefined;
    history.value = [];
    loading.value = false;
    error.value = '';
  }
  function connect(e: string, r?: CommentResource) {
    close();
    epoch = e;
    resource = r;
  }
  async function load(
    target = query.value,
    navigation: 'refresh' | 'next' | 'back' | 'reset' = 'refresh',
  ) {
    if (!epoch || !resource || !target) return;
    const e = epoch,
      g = generation,
      n = ++reading,
      r = resource;
    loading.value = true;
    error.value = '';
    let result: CommentFloorPage | undefined;
    try {
      const value = await core.commentsFloor(e, r, target, true);
      if (value.epoch === e) result = value.value;
    } catch (value) {
      if (live(e, g, n)) {
        const p = failure(value);
        error.value = p.message;
        if (['session_expired', 'profile_changed'].includes(p.code)) {
          close();
          epoch = '';
          expired();
          return;
        }
        try {
          const value = await core.commentsFloor(e, r, target, false);
          if (value.epoch === e) result = value.value;
        } catch {
          /* keep read error */
        }
      }
    } finally {
      if (live(e, g, n)) {
        loading.value = false;
        if (
          result &&
          resourceKey(result.resource) === resourceKey(r) &&
          result.query.parentId === target.parentId &&
          result.query.time === target.time
        ) {
          if (navigation === 'next' && query.value) history.value = [...history.value, query.value];
          if (navigation === 'back') history.value = history.value.slice(0, -1);
          if (navigation === 'reset') history.value = [];
          page.value = result;
          query.value = target;
        }
      }
    }
  }
  async function open(item: CommentItem) {
    close();
    query.value = { parentId: item.parentId ?? item.id, time: -1, limit: 20 };
    await load(query.value, 'reset');
  }
  return {
    page,
    query,
    history,
    loading,
    error,
    connect,
    close,
    open,
    load,
    next: () => page.value?.next && load(page.value.next, 'next'),
    back: () => history.value.length && load(history.value.at(-1)!, 'back'),
  };
}
export type CommentFloorModel = ReturnType<typeof useCommentFloor>;
